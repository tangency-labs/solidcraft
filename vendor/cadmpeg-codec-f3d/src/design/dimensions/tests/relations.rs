// SPDX-License-Identifier: Apache-2.0
use super::project_dimension_constraints;
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::dimensions::exact_atomic_constraint;
use crate::design::dimensions::exact_counted_dimension_relation;
use crate::design::dimensions::exact_offset_constraint;
use crate::design::dimensions::indirect_angular_lines;
use crate::design::test_support::parameter_record;
use crate::ids::neutral_parameter_id_parts;
use crate::ids::neutral_sketch_id;
use crate::records::dimensions::DesignDimensionAnnotationFrame;
use crate::records::dimensions::DesignDimensionAnnotationOperand;
use crate::records::dimensions::DesignDimensionLocus;
use crate::records::dimensions::DesignDimensionLocusGroup;
use crate::records::parameters::DesignParameterCompanion;
use crate::records::sketch_geometry::SketchCurveGeometry;
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_geometry::SketchPoint;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::sketch_relations::SketchConstraintKind;
use crate::records::sketch_relations::SketchRelation;
use crate::records::sketch_relations::SketchRelationOperand;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::Angle;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::SketchAxis;
use cadmpeg_ir::sketches::SketchConstraintDefinitionInput;
use cadmpeg_ir::sketches::SketchEntity;
use cadmpeg_ir::sketches::SketchEntityId;
use cadmpeg_ir::sketches::SketchGeometry;
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::sketches::SketchId;
use cadmpeg_ir::sketches::SketchLocus;
use std::collections::HashMap;

#[test]
fn three_member_symmetry_states_project_unique_reflection_axis() {
    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let first = entity(
        "generated:test:point#left",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(-2.0, 3.0),
        })
        .unwrap(),
    );
    let axis_entity = entity(
        "generated:test:line#axis",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, -5.0),
            end: Point2::new(0.0, 5.0),
        })
        .unwrap(),
    );
    let second = entity(
        "generated:test:point#right",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(2.0, 3.0),
        })
        .unwrap(),
    );

    for kind in [
        SketchConstraintKind::Concentric,
        SketchConstraintKind::Symmetry,
    ] {
        let definition = exact_atomic_constraint(kind, &[&first, &axis_entity, &second]).unwrap();
        assert!(matches!(
            definition,
            SketchConstraintDefinitionInput::Symmetric {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(ref first_id),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(ref second_id),
                axis: ref axis_id,
            } if first_id == first.id()
                && second_id == second.id()
                && axis_id == axis_entity.id()
        ));
    }

    let off_axis = entity(
        "generated:test:line#off-axis",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(1.0, -5.0),
            end: Point2::new(1.0, 5.0),
        })
        .unwrap(),
    );
    assert!(exact_atomic_constraint(
        SketchConstraintKind::Concentric,
        &[&first, &off_axis, &second]
    )
    .is_none());
    let on_axis = entity(
        "generated:test:point#on-axis",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 3.0),
        })
        .unwrap(),
    );
    for kind in [
        SketchConstraintKind::Concentric,
        SketchConstraintKind::Symmetry,
    ] {
        assert!(exact_atomic_constraint(kind, &[&on_axis, &axis_entity, &on_axis]).is_none());
    }
}

#[test]
fn counted_dimension_groups_resolve_full_circle_symmetry() {
    let entity = |id: &str, geometry: SketchGeometry| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let first = entity(
        "generated:test:circle#first",
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(-3.0, 2.0),
            radius: Length::new(1.5).unwrap(),
        })
        .unwrap(),
    );
    let axis = entity(
        "generated:test:line#axis",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, -1.0),
            end: Point2::new(0.0, 4.0),
        })
        .unwrap(),
    );
    let second = entity(
        "generated:test:circle#second",
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(3.0, 2.0),
            radius: Length::new(1.5).unwrap(),
        })
        .unwrap(),
    );

    assert!(matches!(
        exact_counted_dimension_relation(&[&first, &axis, &second]),
        Some(SketchConstraintDefinitionInput::Symmetric {
            first: SketchLocus::Entity(ref first_id),
            second: SketchLocus::Entity(ref second_id),
            axis: ref axis_id,
        }) if first_id == first.id() && second_id == second.id() && axis_id == axis.id()
    ));

    let mut mismatched = second.clone();
    mismatched.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(3.0, 2.0),
        radius: Length::new(2.0).unwrap(),
    })
    .unwrap();
    assert!(exact_counted_dimension_relation(&[&first, &axis, &mismatched]).is_none());
}

#[test]
fn counted_dimension_groups_resolve_bounded_arc_symmetry() {
    let entity = |id: &str, geometry: SketchGeometry| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let first = entity(
        "generated:test:arc#first",
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(-3.0, 2.0),
            radius: Length::new(1.5).unwrap(),
            start_angle: Angle::new(-std::f64::consts::FRAC_PI_4).unwrap(),
            end_angle: Angle::new(std::f64::consts::FRAC_PI_3).unwrap(),
        })
        .unwrap(),
    );
    let axis = entity(
        "generated:test:line#axis",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, -1.0),
            end: Point2::new(0.0, 4.0),
        })
        .unwrap(),
    );
    let second = entity(
        "generated:test:arc#second",
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(3.0, 2.0),
            radius: Length::new(1.5).unwrap(),
            start_angle: Angle::new(2.0 * std::f64::consts::FRAC_PI_3).unwrap(),
            end_angle: Angle::new(5.0 * std::f64::consts::FRAC_PI_4).unwrap(),
        })
        .unwrap(),
    );

    assert!(matches!(
        exact_counted_dimension_relation(&[&first, &axis, &second]),
        Some(SketchConstraintDefinitionInput::Symmetric {
            first: SketchLocus::Entity(ref first_id),
            second: SketchLocus::Entity(ref second_id),
            axis: ref axis_id,
        }) if first_id == first.id() && second_id == second.id() && axis_id == axis.id()
    ));

    let mut mismatched = second.clone();
    mismatched.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(3.0, 2.0),
        radius: Length::new(1.5).unwrap(),
        start_angle: Angle::new(2.0 * std::f64::consts::FRAC_PI_3).unwrap(),
        end_angle: Angle::new(5.0 * std::f64::consts::FRAC_PI_4 + 0.1).unwrap(),
    })
    .unwrap();
    assert!(exact_counted_dimension_relation(&[&first, &axis, &mismatched]).is_none());
}

#[test]
fn counted_dimension_groups_resolve_centered_entities() {
    let entity = |id: &str, geometry: SketchGeometry| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let circle = entity(
        "generated:test:circle#first",
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length::new(3.0).unwrap(),
        })
        .unwrap(),
    );
    let arc = entity(
        "generated:test:arc#second",
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(1.0, 2.0),
            radius: Length::new(2.0).unwrap(),
            start_angle: Angle::new(0.0).unwrap(),
            end_angle: Angle::new(1.0).unwrap(),
        })
        .unwrap(),
    );
    assert!(matches!(
        exact_counted_dimension_relation(&[&circle, &arc]),
        Some(SketchConstraintDefinitionInput::Concentric { first, second })
            if first == circle.id().clone() && second == arc.id().clone()
    ));

    let coradial = entity(
        "generated:test:circle#coradial",
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length::new(3.0).unwrap(),
        })
        .unwrap(),
    );
    assert!(matches!(
        exact_counted_dimension_relation(&[&circle, &coradial]),
        Some(SketchConstraintDefinitionInput::Coradial { first, second })
            if first == circle.id().clone() && second == coradial.id().clone()
    ));

    let ellipse = entity(
        "generated:test:ellipse#same-center",
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(1.0, 2.0),
            major_angle: Angle::new(0.25).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(4.0).unwrap(),
                minor_radius: Length::new(1.5).unwrap(),
            },
            bounds: None,
        })
        .unwrap(),
    );
    assert!(matches!(
        exact_counted_dimension_relation(&[&circle, &ellipse]),
        Some(SketchConstraintDefinitionInput::Concentric { first, second })
            if first == circle.id().clone() && second == ellipse.id().clone()
    ));

    let mut displaced = arc.clone();
    displaced.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(1.0, 2.1),
        radius: Length::new(2.0).unwrap(),
        start_angle: Angle::new(0.0).unwrap(),
        end_angle: Angle::new(1.0).unwrap(),
    })
    .unwrap();
    assert!(exact_counted_dimension_relation(&[&circle, &displaced]).is_none());
}

#[test]
fn coincident_relation_projects_one_unique_shared_locus_per_member() {
    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let line = entity(
        "generated:test:line#0",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(1.0, 2.0),
            end: Point2::new(4.0, 2.0),
        })
        .unwrap(),
    );
    let point = entity(
        "generated:test:point#0",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 2.0),
        })
        .unwrap(),
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::dimensions::exact_coincident_loci(&[&line, &point], decode_ctx)
        })
        .unwrap(),
        Some(SketchConstraintDefinitionInput::CoincidentLoci {
            loci: vec![
                cadmpeg_ir::sketches::SketchLocus::Start(line.id().clone()),
                cadmpeg_ir::sketches::SketchLocus::Entity(point.id().clone()),
            ],
        })
    );

    let degenerate = entity(
        "generated:test:line#degenerate",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(1.0, 2.0),
            end: Point2::new(1.0, 2.0),
        })
        .unwrap(),
    );
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::dimensions::exact_coincident_loci(&[&degenerate, &point], decode_ctx)
    })
    .unwrap()
    .is_none());
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::dimensions::exact_coincident_loci(&[&line, &line], decode_ctx)
    })
    .unwrap()
    .is_none());
    assert!(exact_atomic_constraint(SketchConstraintKind::Coincident, &[&line, &line]).is_none());
}

#[test]
fn polygon_constraint_requires_three_distinct_resolved_members() {
    let entity = |id: &str| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        )
    };
    let first = entity("generated:test:point#0");
    let second = entity("generated:test:point#1");
    let third = entity("generated:test:point#2");
    assert_eq!(
        exact_atomic_constraint(SketchConstraintKind::Polygon, &[&first, &second, &third]),
        Some(SketchConstraintDefinitionInput::Polygon {
            polygon: cadmpeg_ir::sketches::SketchPolygon::try_new(vec![
                first.id().clone(),
                second.id().clone(),
                third.id().clone()
            ])
            .expect("fixture collection admission")
        })
    );
    assert!(exact_atomic_constraint(SketchConstraintKind::Polygon, &[&first, &second]).is_none());
    assert!(
        exact_atomic_constraint(SketchConstraintKind::Polygon, &[&first, &second, &first])
            .is_none()
    );
}

#[test]
fn aggregate_offset_relation_projects_ordered_oriented_pairs() {
    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let source_horizontal = entity(
        "generated:test:line#source-horizontal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
        .unwrap(),
    );
    let result_horizontal = entity(
        "generated:test:line#result-horizontal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(2.0, -2.0),
            end: Point2::new(8.0, -2.0),
        })
        .unwrap(),
    );
    let source_vertical = entity(
        "generated:test:line#source-vertical",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 10.0),
            end: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    let result_vertical = entity(
        "generated:test:line#result-vertical",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(2.0, 2.0),
            end: Point2::new(2.0, 8.0),
        })
        .unwrap(),
    );
    let curve = |record_index, secondary_id| SketchRelationOperand::Curve {
        record_index,
        primary_id: u64::from(record_index),
        secondary_id,
    };
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:native:sketch-relation#0".into(),
        record_index: 10,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 100,
        owner_reference: 1,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(vec![
            crate::records::identity::Located {
                value: 0,
                offset: 80,
            },
        ]),
        rectangular_counted_reference_count: None,
        members: ([(1, 25, 3), (2, 40, 5), (3, 55, 1), (4, 70, 1)]
            .into_iter()
            .map(|(record_index, offset, relation_ordinal)| {
                crate::records::sketch_relations::SketchRelationMember {
                    reference: crate::records::sketch_relations::SketchRelationReference::Index(
                        record_index,
                    ),
                    offset,
                    relation_ordinal: Some(relation_ordinal),
                }
            })
            .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 90,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x20_0000_0000,
            None,
        )
        .expect("valid relation definition"),
        entity_genesis: None,
        return_members: ([
            (1, 120, curve(1, 10)),
            (3, 131, curve(3, 30)),
            (2, 142, curve(2, 20)),
            (4, 153, curve(4, 40)),
        ]
        .into_iter()
        .map(|(_record_index, offset, resolved)| {
            crate::records::sketch_relations::SketchRelationReturnMember {
                reference: crate::records::sketch_relations::SketchRelationReference::Resolved(
                    resolved,
                ),
                offset,
            }
        })
        .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        raw_bytes: vec![0; 160],
    })
    .unwrap();
    let projected = HashMap::from([
        (("native", 1), &source_horizontal),
        (("native", 2), &source_vertical),
        (("native", 3), &result_horizontal),
        (("native", 4), &result_vertical),
    ]);

    let definition = exact_offset_constraint(&relation, "native", &projected).unwrap();
    let SketchConstraintDefinitionInput::Offset {
        pairs,
        distance,
        parameter,
    } = definition
    else {
        panic!("expected neutral offset constraint")
    };
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].source, source_horizontal.id().clone());
    assert_eq!(pairs[0].result, result_horizontal.id().clone());
    assert_eq!(pairs[1].source, source_vertical.id().clone());
    assert_eq!(pairs[1].result, result_vertical.id().clone());
    assert!((distance.get() - 2.0).abs() <= 1.0e-9);
    assert!(pairs[0].source_reversed);
    assert!(!pairs[1].source_reversed);
    assert_eq!(parameter, None);

    let mut repeated_pair = relation;
    let mut returned = repeated_pair.return_members().to_vec();
    returned.extend([
        crate::records::sketch_relations::SketchRelationReturnMember {
            reference: crate::records::sketch_relations::SketchRelationReference::Resolved(curve(
                1, 10,
            )),
            offset: 0,
        },
        crate::records::sketch_relations::SketchRelationReturnMember {
            reference: crate::records::sketch_relations::SketchRelationReference::Resolved(curve(
                3, 30,
            )),
            offset: 0,
        },
    ]);
    repeated_pair
        .try_edit(|draft| {
            draft.return_members = returned.try_into().expect("uniform member resolution");
        })
        .unwrap();
    assert!(exact_offset_constraint(&repeated_pair, "native", &projected).is_none());
}

/// A stored offset relation with `state` whose member and return runs both
/// list `pairs` as consecutive `(source, result)` curve records.
fn stored_offset_relation(state: u64, pairs: &[(u32, u32)]) -> SketchRelation {
    let records = pairs
        .iter()
        .flat_map(|(source, result)| [*source, *result])
        .collect::<Vec<_>>();
    SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:native:sketch-relation#0".into(),
        record_index: 10,
        class_tag: crate::records::references::DesignClassTag::try_from("402".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 100,
        owner_reference: 1,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
        rectangular_counted_reference_count: None,
        members: (records
            .iter()
            .zip((25..).step_by(15))
            .map(
                |(record_index, offset)| crate::records::sketch_relations::SketchRelationMember {
                    reference: crate::records::sketch_relations::SketchRelationReference::Index(
                        *record_index,
                    ),
                    offset,
                    relation_ordinal: Some(0),
                },
            )
            .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 90,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(state, None)
            .expect("valid relation definition"),
        entity_genesis: None,
        return_members: (records
            .iter()
            .zip((120..).step_by(11))
            .map(|(record_index, offset)| {
                crate::records::sketch_relations::SketchRelationReturnMember {
                    reference: crate::records::sketch_relations::SketchRelationReference::Resolved(
                        SketchRelationOperand::Curve {
                            record_index: *record_index,
                            primary_id: u64::from(*record_index),
                            secondary_id: u64::from(*record_index) * 10,
                        },
                    ),
                    offset,
                }
            })
            .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        raw_bytes: vec![0; 160],
    })
    .unwrap()
}

fn offset_test_entity(id: &str, definition: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(id).unwrap(),
        SketchId::mint("generated:test:sketch#0").unwrap(),
        SketchGeometry::try_from(definition).unwrap(),
    )
}

#[test]
fn stored_offset_relation_projects_one_concentric_circle_pair() {
    let source = offset_test_entity(
        "generated:test:circle#source",
        SketchGeometryDefinition::Circle {
            center: Point2::new(4.0, -1.0),
            radius: Length::new(5.0).unwrap(),
        },
    );
    let result = offset_test_entity(
        "generated:test:circle#result",
        SketchGeometryDefinition::Circle {
            center: Point2::new(4.0, -1.0),
            radius: Length::new(3.5).unwrap(),
        },
    );
    let projected = HashMap::from([(("native", 1), &source), (("native", 2), &result)]);
    let relation = stored_offset_relation(0x20_0000_0000, &[(1, 2)]);

    let Some(SketchConstraintDefinitionInput::Offset {
        pairs,
        distance,
        parameter,
    }) = exact_offset_constraint(&relation, "native", &projected)
    else {
        panic!("expected neutral offset constraint")
    };
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].source, source.id().clone());
    assert_eq!(pairs[0].result, result.id().clone());
    assert!(!pairs[0].source_reversed);
    assert!((distance.get() - 1.5).abs() <= 1.0e-9);
    assert_eq!(parameter, None);

    let outward = offset_test_entity(
        "generated:test:circle#outward",
        SketchGeometryDefinition::Circle {
            center: Point2::new(4.0, -1.0),
            radius: Length::new(6.5).unwrap(),
        },
    );
    let projected = HashMap::from([(("native", 1), &source), (("native", 2), &outward)]);
    let Some(SketchConstraintDefinitionInput::Offset {
        pairs, distance, ..
    }) = exact_offset_constraint(&relation, "native", &projected)
    else {
        panic!("expected neutral offset constraint")
    };
    assert!(pairs[0].source_reversed);
    assert!((distance.get() - 1.5).abs() <= 1.0e-9);
}

#[test]
fn stored_offset_relation_projects_a_mixed_line_and_arc_chain() {
    let source_line = offset_test_entity(
        "generated:test:line#source",
        SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
    );
    let result_line = offset_test_entity(
        "generated:test:line#result",
        SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(10.0, 2.0),
        },
    );
    let source_arc = offset_test_entity(
        "generated:test:arc#source",
        SketchGeometryDefinition::Arc {
            center: Point2::new(10.0, 5.0),
            radius: Length::new(5.0).unwrap(),
            start_angle: Angle::new(-std::f64::consts::FRAC_PI_2).unwrap(),
            end_angle: Angle::new(0.0).unwrap(),
        },
    );
    let result_arc = offset_test_entity(
        "generated:test:arc#result",
        SketchGeometryDefinition::Arc {
            center: Point2::new(10.0, 5.0),
            radius: Length::new(3.0).unwrap(),
            start_angle: Angle::new(-std::f64::consts::FRAC_PI_2).unwrap(),
            end_angle: Angle::new(0.0).unwrap(),
        },
    );
    let projected = HashMap::from([
        (("native", 1), &source_line),
        (("native", 2), &result_line),
        (("native", 3), &source_arc),
        (("native", 4), &result_arc),
    ]);
    let relation = stored_offset_relation(0x20_0000_0000, &[(1, 2), (3, 4)]);

    let Some(SketchConstraintDefinitionInput::Offset {
        pairs, distance, ..
    }) = exact_offset_constraint(&relation, "native", &projected)
    else {
        panic!("expected neutral offset constraint")
    };
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[1].source, source_arc.id().clone());
    assert_eq!(pairs[1].result, result_arc.id().clone());
    assert!(!pairs[0].source_reversed);
    assert!(!pairs[1].source_reversed);
    assert!((distance.get() - 2.0).abs() <= 1.0e-9);

    let far_arc = offset_test_entity(
        "generated:test:arc#far",
        SketchGeometryDefinition::Arc {
            center: Point2::new(10.0, 5.0),
            radius: Length::new(2.0).unwrap(),
            start_angle: Angle::new(-std::f64::consts::FRAC_PI_2).unwrap(),
            end_angle: Angle::new(0.0).unwrap(),
        },
    );
    let projected = HashMap::from([
        (("native", 1), &source_line),
        (("native", 2), &result_line),
        (("native", 3), &source_arc),
        (("native", 4), &far_arc),
    ]);
    assert!(exact_offset_constraint(&relation, "native", &projected).is_none());
}

#[test]
fn single_pair_offsets_require_the_offset_bit_and_a_shared_center() {
    let source = offset_test_entity(
        "generated:test:circle#source",
        SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(5.0).unwrap(),
        },
    );
    let displaced = offset_test_entity(
        "generated:test:circle#displaced",
        SketchGeometryDefinition::Circle {
            center: Point2::new(0.5, 0.0),
            radius: Length::new(3.0).unwrap(),
        },
    );
    let projected = HashMap::from([(("native", 1), &source), (("native", 2), &displaced)]);
    let relation = stored_offset_relation(0x20_0000_0000, &[(1, 2)]);
    assert!(exact_offset_constraint(&relation, "native", &projected).is_none());

    let source_line = offset_test_entity(
        "generated:test:line#source",
        SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
    );
    let result_line = offset_test_entity(
        "generated:test:line#result",
        SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(10.0, 2.0),
        },
    );
    let projected = HashMap::from([(("native", 1), &source_line), (("native", 2), &result_line)]);
    let perpendicular = stored_offset_relation(0x20, &[(1, 2)]);
    assert!(exact_offset_constraint(&perpendicular, "native", &projected).is_none());
}

#[test]
fn midpoint_relation_resolves_a_point_at_the_middle_angle_of_an_arc() {
    use crate::design::dimensions::{midpoint_constraint, MidpointCarrier};

    let arc = offset_test_entity(
        "generated:test:arc#carrier",
        SketchGeometryDefinition::Arc {
            center: Point2::new(61.0, -15.0),
            radius: Length::new(5.0).unwrap(),
            start_angle: Angle::new(std::f64::consts::PI).unwrap(),
            end_angle: Angle::new(2.0 * std::f64::consts::PI).unwrap(),
        },
    );
    let point = |id: &str, u: f64, v: f64| {
        offset_test_entity(
            id,
            SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            },
        )
    };
    let middle = point("generated:test:point#middle", 61.0, -20.0);
    let Some(SketchConstraintDefinitionInput::Midpoint {
        point: locus,
        entity,
    }) = midpoint_constraint(&[&middle, &arc], MidpointCarrier::LineOrArc)
    else {
        panic!("expected an arc midpoint constraint")
    };
    assert_eq!(locus, SketchLocus::Entity(middle.id().clone()));
    assert_eq!(entity, arc.id().clone());

    let end = point("generated:test:point#end", 66.0, -15.0);
    assert!(midpoint_constraint(&[&arc, &end], MidpointCarrier::LineOrArc).is_none());
    assert!(midpoint_constraint(&[&arc, &middle], MidpointCarrier::Line).is_none());
}

#[test]
fn single_curve_annotation_projects_parameterized_offset() {
    let stream = "f3d:Design/BulkStream.dat";
    let sketch = SketchId::mint("generated:test:sketch#offset").unwrap();
    let source_curve_id = format!("{stream}:sketch-curve#10");
    let result_curve_id = format!("{stream}:sketch-curve#11");
    let curve = |id: String, record_index, primary_id, secondary_id| SketchCurveIdentity {
        id,
        record_index,
        owner_reference: Some(100),
        class_tag: crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(primary_id).unwrap(),
        secondary_id,
        geometry: None,
        planar_z_noise: None,
    };
    let source_curve = curve(source_curve_id.clone(), 10, 20, 0);
    let result_curve = curve(result_curve_id.clone(), 11, 21, 7);
    let entity = |id: &str, native_ref: String, start, end| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
        .with_native_ref(Some(native_ref))
    };
    let source = entity(
        "synthetic:test:id#source",
        source_curve_id,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let result = entity(
        "synthetic:test:id#result",
        result_curve_id,
        Point2::new(0.0, -2.0),
        Point2::new(10.0, -2.0),
    );
    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: format!("{stream}:design-parameter#12"),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                .unwrap(),
            record_index: 12,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::new(
                "Linear Dimension-2".into(),
                Some(13),
                Some(crate::records::identity::Located {
                    value: crate::records::parameters::DesignParameterDiscriminator::Code6,
                    offset: 22,
                }),
            )
            .unwrap(),
            expression: "2 mm".into(),
            expression_offset: 40,
            source_kind_offset: 60,

            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 70,
            }),
            name: "d1".into(),
            name_offset: 80,
            evaluated_value: 0.2,
            evaluated_value_offset: 90,
        },
    )
    .unwrap();
    let frame = DesignDimensionAnnotationFrame::try_new(
        crate::records::dimensions::DesignDimensionAnnotationFrameDraft {
            id: format!("{stream}:design-dimension-annotation-frame#14"),
            companion_record_index: Some(15),
            governing_companion_record_index: 15,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned())
                .unwrap(),
            record_index: 14,
            frame_length: 100,
            operands: vec![
                DesignDimensionAnnotationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(0),
                    geometry_reference_offset: 25,
                    role: 3,
                    role_offset: 35,
                },
                DesignDimensionAnnotationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(10),
                    geometry_reference_offset: 40,
                    role: 2,
                    role_offset: 50,
                },
            ],
            entity_genesis: Some(0x80),
            annotation_bytes: Vec::new(),
            annotation_byte_offset: 111,
            governing_owner_record_index: 13,
            governing_owner_reference_offset: 112,
            return_members: vec![crate::records::identity::Located {
                value: std::num::NonZeroU32::new(10).unwrap(),
                offset: 127,
            }],
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "256".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 100,
            owner_reference: 100,
            owner_reference_offset: 120,
        },
    )
    .unwrap();
    let parameter_id =
        ParameterId::mint("generated:test:parameter#offset").expect("identity grammar");
    let projected = HashMap::from([((stream, 10), &source), ((stream, 11), &result)]);

    let definition = crate::design::dimensions::annotation_offset_dimension_definition(
        &frame,
        (&parameter, &parameter_id),
        stream,
        &[source_curve.clone(), result_curve.clone()],
        &projected,
        1.0e-6,
    )
    .expect("single-curve annotation offset");
    assert!(matches!(
        definition,
        SketchConstraintDefinitionInput::Offset {
            pairs,
            distance,
            parameter: Some(cadmpeg_ir::sketches::OffsetParameter {
                id: actual_parameter,
                negated: false,
            }),
        } if pairs.as_slice() == [cadmpeg_ir::sketches::SketchOffsetPair {
            source: source.id().clone(),
            result: result.id().clone(),
            source_reversed: true,
        }] && (distance.get() - 2.0).abs() <= 1.0e-9
            && actual_parameter == parameter_id
    ));

    let explicit_frame = DesignDimensionAnnotationFrame::try_new(
        crate::records::dimensions::DesignDimensionAnnotationFrameDraft {
            operands: vec![
                DesignDimensionAnnotationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(0),
                    geometry_reference_offset: 25,
                    role: 3,
                    role_offset: 35,
                },
                DesignDimensionAnnotationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(11),
                    geometry_reference_offset: 40,
                    role: 1,
                    role_offset: 50,
                },
                DesignDimensionAnnotationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(10),
                    geometry_reference_offset: 55,
                    role: 2,
                    role_offset: 65,
                },
            ],
            return_members: vec![
                crate::records::identity::Located {
                    value: std::num::NonZeroU32::new(10).unwrap(),
                    offset: 142,
                },
                crate::records::identity::Located {
                    value: std::num::NonZeroU32::new(11).unwrap(),
                    offset: 153,
                },
            ],
            annotation_byte_offset: 126,
            governing_owner_reference_offset: 127,
            ..frame.clone().into_draft()
        },
    )
    .unwrap();
    let explicit_definition = crate::design::dimensions::annotation_offset_dimension_definition(
        &explicit_frame,
        (&parameter, &parameter_id),
        stream,
        &[source_curve.clone(), result_curve.clone()],
        &projected,
        1.0e-6,
    )
    .expect("explicit two-curve annotation offset");
    assert!(matches!(
        explicit_definition,
        SketchConstraintDefinitionInput::Offset {
            pairs,
            parameter: Some(cadmpeg_ir::sketches::OffsetParameter {
                id: actual_parameter,
                negated: false,
            }),
            ..
        } if pairs.as_slice() == [cadmpeg_ir::sketches::SketchOffsetPair {
            source: source.id().clone(),
            result: result.id().clone(),
            source_reversed: true,
        }] && actual_parameter == parameter_id
    ));

    let duplicate_curve_id = format!("{stream}:sketch-curve#12");
    let duplicate_curve = curve(duplicate_curve_id.clone(), 12, 22, 8);
    let duplicate = entity(
        "synthetic:test:id#duplicate",
        duplicate_curve_id,
        Point2::new(2.0, -2.0),
        Point2::new(8.0, -2.0),
    );
    let projected_with_duplicate = HashMap::from([
        ((stream, 10), &source),
        ((stream, 11), &result),
        ((stream, 12), &duplicate),
    ]);
    assert!(
        crate::design::dimensions::annotation_offset_dimension_definition(
            &frame,
            (&parameter, &parameter_id),
            stream,
            &[
                curve(format!("{stream}:sketch-curve#10"), 10, 20, 0),
                curve(format!("{stream}:sketch-curve#11"), 11, 21, 7),
                duplicate_curve,
            ],
            &projected_with_duplicate,
            1.0e-6
        )
        .is_none()
    );
}

#[test]
fn mixed_circle_arc_offset_uses_concentric_radius_difference() {
    let circle = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(0.0, 0.0),
        radius: Length::new(20.0).unwrap(),
    })
    .unwrap();
    let arc = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length::new(22.0).unwrap(),
        start_angle: Angle::new(-0.2).unwrap(),
        end_angle: Angle::new(0.1).unwrap(),
    })
    .unwrap();
    let distance = crate::design::dimensions::sketch_curve_offset(&circle, &arc)
        .expect("concentric circle-to-arc offset");
    assert!((distance + 2.0).abs() <= 1.0e-9);

    let clockwise_arc = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length::new(22.0).unwrap(),
        start_angle: Angle::new(0.1).unwrap(),
        end_angle: Angle::new(-0.2).unwrap(),
    })
    .unwrap();
    let distance = crate::design::dimensions::sketch_curve_offset(&clockwise_arc, &circle)
        .expect("concentric arc-to-circle offset");
    assert!((distance + 2.0).abs() <= 1.0e-9);

    let displaced_arc = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(1.0e-6, 0.0),
        radius: Length::new(22.0).unwrap(),
        start_angle: Angle::new(0.1).unwrap(),
        end_angle: Angle::new(-0.2).unwrap(),
    })
    .unwrap();
    assert!(crate::design::dimensions::sketch_curve_offset(&circle, &displaced_arc).is_none());
}

#[test]
fn angular_point_operand_selects_unique_incident_line_by_value() {
    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let point = entity(
        "generated:test:point#vertex",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    let explicit = entity(
        "generated:test:line#explicit",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(2.0, -2.0),
            end: Point2::new(2.0, 2.0),
        })
        .unwrap(),
    );
    let diagonal = entity(
        "generated:test:line#diagonal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 2.0),
        })
        .unwrap(),
    );
    let horizontal = entity(
        "generated:test:line#horizontal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        })
        .unwrap(),
    );
    let projected = HashMap::from([
        (("native", 1), &point),
        (("native", 2), &explicit),
        (("native", 3), &diagonal),
        (("native", 4), &horizontal),
    ]);

    let lines = indirect_angular_lines(
        "native",
        &[&point, &explicit],
        std::f64::consts::FRAC_PI_4,
        &projected,
    )
    .unwrap();
    assert_eq!(lines, (diagonal.id().clone(), explicit.id().clone()));
    let supplementary = indirect_angular_lines(
        "native",
        &[&point, &explicit],
        3.0 * std::f64::consts::FRAC_PI_4,
        &projected,
    )
    .unwrap();
    assert_eq!(supplementary, lines);
    let duplicate_diagonal = entity(
        "generated:test:line#duplicate-diagonal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(4.0, 4.0),
        })
        .unwrap(),
    );
    let projected_with_duplicate = HashMap::from([
        (("native", 1), &point),
        (("native", 2), &explicit),
        (("native", 3), &diagonal),
        (("native", 4), &horizontal),
        (("native", 5), &duplicate_diagonal),
    ]);
    assert!(indirect_angular_lines(
        "native",
        &[&point, &explicit],
        std::f64::consts::FRAC_PI_4,
        &projected_with_duplicate
    )
    .is_none());
}

#[test]
fn counted_angular_group_projects_unique_point_selected_line() {
    let stream = "f3d:A";
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),

        id: format!("{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: crate::records::identity::DesignEntityId::try_from("0_100".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("356".to_owned()).unwrap(),
        record_index: 11,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
    };
    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: format!("{stream}:design-parameter#20"),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                .unwrap(),
            record_index: 20,
            source_ordinal: 4,
            source: crate::records::parameters::DesignParameterSource::new(
                "Angular Dimension-4".into(),
                Some(21),
                Some(crate::records::identity::Located {
                    value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                    offset: 22,
                }),
            )
            .unwrap(),
            expression: "1.0471975512 rad".into(),
            expression_offset: 40,
            source_kind_offset: 60,

            unit: Some(crate::records::identity::RecordedValue {
                value: "rad".into(),
                offset: 70,
            }),
            name: "d4".into(),
            name_offset: 80,
            evaluated_value: std::f64::consts::FRAC_PI_3,
            evaluated_value_offset: 90,
        },
    )
    .unwrap();
    let owner = crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: format!("{stream}:design-parameter-owner#21"),
            byte_offset: 0,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 21,
            scope_record_index: 10,
            local_ordinal: 0,
            evaluated_value: std::f64::consts::FRAC_PI_3,
            evaluated_value_offset: 40,
            parameter_record_index: 20,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index: 22,
        },
    )
    .unwrap();
    let companion = DesignParameterCompanion::unbound(
        format!("{stream}:design-parameter-companion#22"),
        0,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        22,
        21,
        std::num::NonZeroU64::new(1).unwrap(),
        42,
    )
    .bound(crate::records::parameters::DesignCompanionPayload::new(
        58,
        0,
        Vec::new(),
    ));
    let group = DesignDimensionLocusGroup {
        id: format!("{stream}:design-dimension-locus-group#30"),
        companion_record_index: 22,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap(),
        record_index: 30,
        frame_length: 100,
        loci: vec![
            DesignDimensionLocus {
                returned: crate::records::identity::Located {
                    value: 40,
                    offset: 0,
                },
                geometry_record_index: 40,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
            DesignDimensionLocus {
                returned: crate::records::identity::Located {
                    value: 41,
                    offset: 0,
                },
                geometry_record_index: 41,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
        ],
        owner_reference: 100,
        owner_reference_offset: 0,
        owner_role: 1,
        owner_role_offset: 0,
        state: 0,
        state_offset: 0,
        next_class_tag: crate::records::references::DesignClassTag::try_from("273".to_owned())
            .unwrap(),
        next_record_index: 31,
        next_byte_offset: 100,
    };
    let point = SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
        id: format!("{stream}:sketch-point#40"),
        record_index: 40,
        owner_reference: Some(100),
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        coordinate_offset: 0,
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
            40,
            crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
            None,
            0.0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(0.0, 0.0),
    })
    .unwrap();
    let curve = |record_index: u32, start: Point2, end: Point2| {
        let delta_u = end.u - start.u;
        let delta_v = end.v - start.v;
        let length = delta_u.hypot(delta_v);
        SketchCurveIdentity {
            id: format!("{stream}:sketch-curve#{record_index}"),
            record_index,
            owner_reference: Some(100),
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            byte_offset: 0,
            geometry_offset: 0,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(u64::from(record_index)).unwrap(),
            secondary_id: 0,
            geometry: Some(
                SketchCurveGeometry::line(
                    Point3::new(start.u, start.v, 0.0),
                    Point3::new(end.u, end.v, 0.0),
                    Vector3::new(delta_u / length, delta_v / length, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            planar_z_noise: None,
        }
    };
    let explicit = curve(41, Point2::new(0.0, 0.0), Point2::new(2.0, 0.0));
    let candidate = curve(42, Point2::new(0.0, 0.0), Point2::new(1.0, 3.0f64.sqrt()));
    let sketch = neutral_sketch_id(&placement);
    let point_entity = SketchEntity::new(
        SketchEntityId::mint("generated:test:point#40").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: point.coordinates(),
        })
        .unwrap(),
    )
    .with_native_ref(Some(point.id.clone()));
    let explicit_entity = SketchEntity::new(
        SketchEntityId::mint("generated:test:line#41").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        })
        .unwrap(),
    )
    .with_native_ref(Some(explicit.id.clone()));
    let candidate_entity = SketchEntity::new(
        SketchEntityId::mint("generated:test:line#42").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 3.0f64.sqrt()),
        })
        .unwrap(),
    )
    .with_native_ref(Some(candidate.id.clone()));
    let entities = vec![point_entity, explicit_entity, candidate_entity];
    let curves = vec![explicit, candidate];
    let constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: std::slice::from_ref(&point),
            curves: &curves,
            entities: &entities,
        },
        &[],
    );

    assert_eq!(constraints.len(), 1);
    assert!(matches!(
        constraints[0].definition.kind(),
        SketchConstraintDefinitionInput::Angle {
            first,
            second,
            parameter: actual_parameter,
        } if first == entities[2].id()
            && second == entities[1].id()
            && actual_parameter == &neutral_parameter_id_parts(stream, 20)
    ));
}

#[test]
fn parallel_group_binds_one_common_axis_angle() {
    let line = |id: &str, end: Point2| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end,
            })
            .unwrap(),
        )
    };
    let first = line("generated:test:line#first", Point2::new(1.0, 1.0));
    let second = line("generated:test:line#second", Point2::new(-2.0, -2.0));
    let mismatch = line("generated:test:line#mismatch", Point2::new(1.0, 0.0));
    let crossed = line("generated:test:line#crossed", Point2::new(1.0, -1.0));
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(44),
        "45 deg",
        "Angular Dimension-2",
        Some("deg"),
        "d1",
        std::f64::consts::FRAC_PI_4,
    ))
    .expect("generated angular dimension is canonical");
    let parameter_id =
        ParameterId::mint("generated:test:parameter#axis-angle").expect("identity grammar");

    assert!(matches!(
        crate::design::dimensions::parallel_group_axis_angle_definition(&[&first, &second], &parameter, &parameter_id),
        Some(SketchConstraintDefinitionInput::AngleToAxis {
            entity,
            axis: SketchAxis::Horizontal,
            parameter,
        }) if entity == first.id().clone() && parameter == parameter_id
    ));
    assert!(
        crate::design::dimensions::parallel_group_axis_angle_definition(
            &[&first, &mismatch],
            &parameter,
            &parameter_id
        )
        .is_none()
    );
    assert!(
        crate::design::dimensions::parallel_group_axis_angle_definition(
            &[&first, &crossed],
            &parameter,
            &parameter_id
        )
        .is_none()
    );
}
